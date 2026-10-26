"""Independent SYNC intent and truthful observations of the actual mapped voice."""

from dataclasses import dataclass, replace
from typing import TYPE_CHECKING, Literal, cast

from flitzis_looper.models import validate_sample_id
from flitzis_looper.source_sync_intent import next_source_sync_intent
from flitzis_looper.timing_policy import variable_source_timing

if TYPE_CHECKING:
    from flitzis_looper.control_diagnostics import ConsoleDiagnostics
    from flitzis_looper.controller.source_beatmap_selection import (
        SourceBeatmapObservation,
        SourceBeatmapSelection,
    )
    from flitzis_looper.controller.transport import TransportController


type SourceSyncGuidance = Literal[
    "pending", "settling", "refused", "restart", "unconfirmed", "phase"
]

_GUIDANCE: dict[SourceSyncGuidance, str] = {
    "pending": "Live timing change pending.",
    "settling": "Timing change applied; tempo is settling.",
    "refused": "Live timing change refused.",
    "restart": "Start/retrigger to apply this timing change.",
    "unconfirmed": "Audio confirmation unavailable.",
    "phase": "Timing mode applied; grid phase is unconfirmed.",
}
type SourceMapReadiness = Literal["unavailable", "preparing", "failed"]

_MAP_GUIDANCE: dict[SourceMapReadiness, str] = {
    "unavailable": "Variable SYNC requires a qualified source map.",
    "preparing": "Source map is preparing; variable SYNC is waiting.",
    "failed": "Source map preparation failed; variable SYNC is unavailable.",
}
# Exact wire names of Native's bounded RuntimePolicyError enum.
_ADOPTION_REASONS = frozenset({
    "busy",
    "stale",
    "unsupported",
    "source_changed",
    "missing_master",
    "invalid_target",
    "phase_changed",
    "reader_unavailable",
    "capacity",
    "scheduler_full",
    "sequence_exhausted",
    "no_active_voices",
})


@dataclass(frozen=True, slots=True, kw_only=True)
class SourceBirthStatus:
    """Source-bound emission observation; never a device or musical acceptance."""

    requested: Literal["first_sound_at_grid", "attack_at_grid", "mixed"]
    outcome: Literal["complete_beginning_at_e", "mixed"]
    alignment: Literal["pending", "earliest_bound_met", "mixed"]
    reason: Literal["output_unconfirmed", "no_qualified_anchor", "mixed"] | None
    original_target_frame: int | None
    execution_frame: int | None
    reference_qualified_at_birth: bool
    emitted: bool


def _birth_labels_valid(
    requested: object, outcome: object, alignment: object, reason: object
) -> bool:
    if any(type(value) is not str for value in (requested, outcome, alignment)):
        return False
    if reason is not None and type(reason) is not str:
        return False
    return (
        requested in {"first_sound_at_grid", "attack_at_grid", "mixed"}
        and outcome in {"complete_beginning_at_e", "mixed"}
        and alignment in {"pending", "earliest_bound_met", "mixed"}
        and reason in {"output_unconfirmed", "no_qualified_anchor", "mixed", None}
    )


def _parse_birth(value: object) -> SourceBirthStatus | None:
    if value is None:
        return None
    if not isinstance(value, dict) or set(value) != {
        "requested",
        "outcome",
        "alignment",
        "reason",
        "original_target_frame",
        "execution_frame",
        "reference_qualified_at_birth",
        "emitted",
    }:
        message = "Invalid source birth feedback schema."
        raise ValueError(message)
    requested, outcome, alignment, reason = (
        value["requested"],
        value["outcome"],
        value["alignment"],
        value["reason"],
    )
    original, execution = value["original_target_frame"], value["execution_frame"]
    reference, emitted = value["reference_qualified_at_birth"], value["emitted"]
    if (
        not _birth_labels_valid(requested, outcome, alignment, reason)
        or type(reference) is not bool
        or type(emitted) is not bool
        or any(
            frame is not None and (type(frame) is not int or not 0 <= frame < 1 << 64)
            for frame in (original, execution)
        )
    ):
        message = "Invalid source birth feedback fields."
        raise ValueError(message)
    if requested == "mixed":
        valid = (
            outcome == alignment == reason == "mixed"
            and original is None
            and execution is None
            and reference is False
            and emitted is False
        )
    else:
        valid = (
            outcome == "complete_beginning_at_e"
            and type(original) is int
            and type(execution) is int
            and execution >= original
        )
        if requested == "attack_at_grid":
            valid = valid and alignment == "pending" and reason == "no_qualified_anchor"
        elif emitted:
            valid = valid and alignment == "earliest_bound_met" and reason is None
        else:
            valid = valid and alignment == "pending" and reason == "output_unconfirmed"
    if not valid:
        message = "Inconsistent source birth feedback."
        raise ValueError(message)
    return SourceBirthStatus(
        requested=cast('Literal["first_sound_at_grid", "attack_at_grid", "mixed"]', requested),
        outcome=cast('Literal["complete_beginning_at_e", "mixed"]', outcome),
        alignment=cast('Literal["pending", "earliest_bound_met", "mixed"]', alignment),
        reason=cast('Literal["output_unconfirmed", "no_qualified_anchor", "mixed"] | None', reason),
        original_target_frame=original,
        execution_frame=execution,
        reference_qualified_at_birth=reference,
        emitted=emitted,
    )


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
    guidance: SourceSyncGuidance | None = None
    map_readiness: SourceMapReadiness | None = None
    grid_phase_confirmed: bool | None = None
    birth: SourceBirthStatus | None = None

    @property
    def guidance_text(self) -> str | None:
        """Project only fixed human messages, never technical diagnostic detail."""
        if self.birth is not None:
            if self.birth.reason == "no_qualified_anchor":
                return "Attack alignment is pending; playback preserves the complete beginning."
            if self.birth.alignment == "mixed":
                return "Start alignment differs between active voices."
            if not self.birth.emitted:
                return "Start output is not yet confirmed."
        if self.map_readiness is not None:
            return _MAP_GUIDANCE[self.map_readiness]
        return None if self.guidance is None else _GUIDANCE[self.guidance]


def _phase_feedback_valid(feedback: dict[str, object]) -> bool:
    phase_revision = feedback.get("phase_program_revision")
    return all(
        feedback.get(key) is None or type(feedback[key]) is bool
        for key in ("phase_read_confirmed", "phase_program_current")
    ) and (phase_revision is None or (type(phase_revision) is int and 0 < phase_revision < 1 << 64))


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
    if _parse_birth(feedback.get("birth")) is not None and feedback["active"] is not True:
        return False
    target_current = feedback.get("target_current")
    if target_current is not None and type(target_current) is not bool:
        return False
    revision, map_revision = feedback.get("intent_revision"), feedback.get("map_revision")
    return (
        _phase_feedback_valid(feedback)
        and (revision is None or (type(revision) is int and 0 <= revision < 1 << 64))
        and (map_revision is None or isinstance(map_revision, str))
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
    mode = feedback.get("policy_mode")
    if mode is not None and (
        not isinstance(mode, str) or mode not in {"legacy", "off", "fixed", "mapped"}
    ):
        return False
    return (
        "policy_mode" in feedback
        and "policy_revision" in feedback
        and (revision is None or (type(revision) is int and 0 <= revision < 1 << 64))
        and "settled" in feedback
        and (feedback["settled"] is None or type(feedback["settled"]) is bool)
    )


def _actual_realtime_bpm(feedback: dict[str, object]) -> bool | None:
    if feedback.get("policy_mode") is None or feedback.get("settled") is not True:
        return None
    return feedback["policy_mode"] == "mapped" and feedback["policy_realtime_bpm"] is not False


def _adoption_state_valid(adoption: dict[str, object]) -> bool:
    state, mode, reason = adoption["state"], adoption["target_mode"], adoption["reason"]
    if not isinstance(state, str) or state not in {"pending", "applied", "refused"}:
        return False
    if not isinstance(mode, str) or mode not in {"legacy", "off", "fixed", "mapped"}:
        return False
    if reason is not None and (not isinstance(reason, str) or reason not in _ADOPTION_REASONS):
        return False
    if state == "pending" and (reason is not None or adoption["execution_frame"] is not None):
        return False
    return not (state == "applied" and (reason is not None or adoption["execution_frame"] is None))


def _adoption_fields_valid(adoption: dict[str, object]) -> bool:
    keys = {
        "snapshot_revision",
        "attempt",
        "state",
        "reason",
        "execution_frame",
        "target_mode",
        "target_policy_revision",
        "target_policy_sync",
        "target_policy_realtime_bpm",
        "target_intent_revision",
    }
    if not keys.issubset(adoption):
        return False
    for key in ("snapshot_revision", "attempt", "target_intent_revision"):
        value = adoption[key]
        if type(value) is not int or not 0 <= value < 1 << 64:
            return False
    for key in ("execution_frame", "target_policy_revision"):
        value = adoption[key]
        if value is not None and (type(value) is not int or not 0 <= value < 1 << 64):
            return False
    for key in ("target_policy_sync", "target_policy_realtime_bpm"):
        value = adoption[key]
        if value is not None and type(value) is not bool:
            return False
    return _adoption_state_valid(adoption)


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
        """Persist one guarded request; Native separately confirms supported adoption."""
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
        if not self._project.resolve_bpm_lock() or self._project.global_timing_policy.sync is False:
            message = "PAD SYNC requires available Multi Loop, BPM Lock and global SYNC."
            raise ValueError(message)
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

    def requested_mode(self, sample_id: int) -> Literal["legacy", "fixed", "mapped"]:
        """Resolve the same mode matrix as Native without granting adoption."""
        policy = self._project.global_timing_policy
        requested = self._project.resolve_pad_source_sync(sample_id)
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
        """Compare actual callback feedback with current intent; readiness grants no ACK."""
        validate_sample_id(sample_id)
        requested = self._project.resolve_pad_source_sync(sample_id)
        try:
            feedback = self._observation(sample_id)
        except (RuntimeError, ValueError, TypeError) as error:
            return SourceSyncStatus(
                requested=requested, state="unconfirmed", detail=str(error), guidance="unconfirmed"
            )
        if feedback is None:
            return SourceSyncStatus(
                requested=requested,
                state="unconfirmed",
                detail="Audio confirmation unavailable.",
                guidance="unconfirmed",
            )
        return replace(
            self._observed_status(sample_id, feedback), birth=_parse_birth(feedback.get("birth"))
        )

    def _observed_status(self, sample_id: int, feedback: dict[str, object]) -> SourceSyncStatus:
        requested = self._project.resolve_pad_source_sync(sample_id)
        map_observation = (
            self._maps._observe_state(sample_id)
            if self.requested_mode(sample_id) == "mapped" and self._maps is not None
            else None
        )
        if not self._project.global_timing_policy.is_neutral or _policy_feedback_valid(feedback):
            return self._policy_status(sample_id, feedback, map_observation)
        adoption = self._adoption_status(sample_id, feedback)
        active, effective = feedback["active"] is True, feedback["effective"] is True
        if not requested:
            if adoption is not None:
                return adoption
            return SourceSyncStatus(
                requested=False,
                effective=effective,
                active=active,
                state="pending" if effective else "off",
                detail="Restart to apply SYNC OFF." if effective else None,
                guidance="restart" if effective else None,
            )
        return self._requested_status(
            sample_id,
            feedback,
            map_observation,
            adoption=adoption,
        )

    def _policy_status(
        self,
        sample_id: int,
        feedback: dict[str, object],
        map_observation: SourceBeatmapObservation | None,
    ) -> SourceSyncStatus:
        requested = self._project.resolve_pad_source_sync(sample_id)
        expected = self.requested_mode(sample_id)
        intent = self._project.pad_source_sync_intent[sample_id]
        policy = self._project.global_timing_policy
        active, effective = feedback["active"] is True, feedback["effective"] is True
        mode = cast("Literal['legacy', 'off', 'fixed', 'mapped'] | None", feedback["policy_mode"])
        matching = (
            mode == expected
            and feedback["policy_revision"] == (None if policy.is_neutral else policy.revision)
            and feedback["policy_sync"] is policy.sync
            and feedback["policy_realtime_bpm"] is policy.realtime_bpm
            and feedback["intent_revision"] == intent.revision
        )
        if expected == "mapped":
            geometry = None if map_observation is None else map_observation.geometry
            matching = (
                matching
                and geometry is not None
                and feedback["map_revision"] == geometry.revision()
            )
        if matching and feedback["settled"] is True and (not requested or effective):
            phase_confirmed = (
                requested
                and active
                and feedback.get("target_current") is True
                and feedback.get("phase_program_current") is True
                and feedback.get("phase_program_revision") is not None
                and feedback.get("phase_read_confirmed") is True
            )
            return SourceSyncStatus(
                requested=requested,
                effective=effective,
                active=active,
                state="effective"
                if phase_confirmed
                else "pending"
                if requested or effective
                else "off",
                policy_mode=mode,
                actual_realtime_bpm=_actual_realtime_bpm(feedback),
                detail="Grid phase is unconfirmed." if requested and not phase_confirmed else None,
                guidance="phase" if requested and not phase_confirmed else None,
                grid_phase_confirmed=True if phase_confirmed else None,
            )
        if (
            matching
            and expected != "mapped"
            and active
            and feedback.get("target_current") is True
            and feedback["settled"] is False
        ):
            return SourceSyncStatus(
                requested=requested,
                effective=effective,
                active=active,
                state="pending",
                policy_mode=mode,
                actual_realtime_bpm=_actual_realtime_bpm(feedback),
                guidance="settling",
            )
        adoption = self._adoption_status(sample_id, feedback)
        if requested and expected == "mapped":
            return self._pending_mapped_policy(sample_id, feedback, map_observation, adoption)
        if adoption is not None:
            return adoption
        status = SourceSyncStatus(
            requested=requested,
            effective=effective,
            active=active,
            state="pending" if active or effective else "stopped" if requested else "off",
            policy_mode=mode,
            actual_realtime_bpm=_actual_realtime_bpm(feedback),
            detail=f"Start/retrigger to apply {expected} timing." if active or requested else None,
            guidance="restart" if active or requested else None,
        )
        pending, error = self._transport.residency.status(sample_id)
        if error:
            status = replace(status, state="refused", detail=error)
        elif pending:
            status = replace(status, detail=f"Playback {pending}.")
        return status

    def _current_adoption(
        self, sample_id: int, feedback: dict[str, object]
    ) -> dict[str, object] | None:
        """Validate Native's current-snapshot observation against the full raw target."""
        adoption = feedback.get("adoption")
        if not isinstance(adoption, dict) or not _adoption_fields_valid(adoption):
            return None
        policy = self._project.global_timing_policy
        intent = self._project.pad_source_sync_intent[sample_id]
        revision = None if policy.is_neutral else policy.revision
        if (
            adoption["target_mode"] != self.requested_mode(sample_id)
            or adoption["target_policy_revision"] != revision
            or adoption["target_policy_sync"] is not policy.sync
            or adoption["target_policy_realtime_bpm"] is not policy.realtime_bpm
            or adoption["target_intent_revision"] != intent.revision
        ):
            return None
        return adoption

    def _adoption_status(
        self, sample_id: int, feedback: dict[str, object]
    ) -> SourceSyncStatus | None:
        adoption = self._current_adoption(sample_id, feedback)
        if adoption is None:
            return None
        active = feedback["active"] is True
        if not active and (
            adoption["state"] == "applied"
            or (adoption["state"] == "refused" and adoption["reason"] == "no_active_voices")
        ):
            # Idle service has no live change to apply. Keep the ordinary stopped/
            # off projection; neither this refusal nor an old applied result is ACK.
            return None
        refused = adoption["state"] == "refused"
        policy_valid = _policy_feedback_valid(feedback)
        guidance: SourceSyncGuidance = (
            "refused" if refused else "settling" if adoption["state"] == "applied" else "pending"
        )
        return SourceSyncStatus(
            requested=self._project.resolve_pad_source_sync(sample_id),
            effective=feedback["effective"] is True,
            active=active,
            state="refused" if refused else "pending",
            detail=(
                f"Live timing adoption refused: {adoption['reason'] or 'unspecified'} "
                f"(snapshot {adoption['snapshot_revision']}, attempt {adoption['attempt']})."
                if refused
                else None
            ),
            policy_mode=(
                cast("Literal['legacy', 'off', 'fixed', 'mapped'] | None", feedback["policy_mode"])
                if policy_valid
                else None
            ),
            actual_realtime_bpm=_actual_realtime_bpm(feedback) if policy_valid else None,
            guidance=guidance,
        )

    def _pending_mapped_policy(
        self,
        sample_id: int,
        feedback: dict[str, object],
        map_observation: SourceBeatmapObservation | None,
        adoption: SourceSyncStatus | None,
    ) -> SourceSyncStatus:
        status = self._requested_status(
            sample_id,
            feedback,
            map_observation,
            adoption=adoption,
        )
        # Matching map/PAD metadata alone cannot acknowledge a different global revision.
        status = replace(
            status,
            policy_mode=cast(
                "Literal['legacy', 'off', 'fixed', 'mapped'] | None", feedback["policy_mode"]
            ),
            actual_realtime_bpm=_actual_realtime_bpm(feedback),
        )
        if status.guidance == "phase":
            return replace(
                status,
                state="pending",
                detail="Start/retrigger to apply the requested global policy.",
                guidance="restart",
            )
        return status

    def _requested_status(
        self,
        sample_id: int,
        feedback: dict[str, object],
        map_observation: SourceBeatmapObservation | None,
        *,
        adoption: SourceSyncStatus | None,
    ) -> SourceSyncStatus:
        active, effective = feedback["active"] is True, feedback["effective"] is True
        intent = self._project.pad_source_sync_intent[sample_id]
        geometry = None if map_observation is None else map_observation.geometry
        if geometry is None:
            return self._unavailable_map_status(sample_id, feedback, map_observation, adoption)
        if adoption is not None:
            return adoption
        matching = (
            effective
            and feedback["intent_revision"] == intent.revision
            and geometry is not None
            and feedback["map_revision"] == geometry.revision()
        )
        if matching:
            return SourceSyncStatus(
                requested=True,
                effective=True,
                active=active,
                state="pending",
                detail="Grid phase is unconfirmed.",
                guidance="phase",
            )
        pending, error = self._transport.residency.status(sample_id)
        if error:
            return SourceSyncStatus(
                requested=True, effective=effective, active=active, state="refused", detail=error
            )
        try:
            self._transport.loop._source_sync_requested_region(sample_id, geometry)
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
            guidance="restart" if pending is None else None,
        )

    def _unavailable_map_status(
        self,
        sample_id: int,
        feedback: dict[str, object],
        map_observation: SourceBeatmapObservation | None,
        adoption: SourceSyncStatus | None,
    ) -> SourceSyncStatus:
        """Explain one qualified map observation while preserving actual audio and refusal."""
        map_state = "none" if map_observation is None else map_observation.status
        map_error = None if map_observation is None else map_observation.error
        readiness: SourceMapReadiness = (
            "preparing"
            if map_state in {"pending", "restoring", "settling"}
            else "failed"
            if map_state == "failed"
            else "unavailable"
        )
        pending, playback_error = self._transport.residency.status(sample_id)
        refused = (
            readiness == "failed"
            or bool(map_error or playback_error)
            or (adoption is not None and adoption.state == "refused")
        )
        details = [map_error or _MAP_GUIDANCE[readiness]]
        if playback_error:
            details.append(playback_error)
        elif pending:
            details.append(f"Playback {pending}.")
        if adoption is not None and adoption.detail is not None:
            details.append(adoption.detail)
        policy_valid = _policy_feedback_valid(feedback)
        return SourceSyncStatus(
            requested=True,
            effective=feedback["effective"] is True,
            active=feedback["active"] is True,
            state="refused" if refused else "pending",
            detail=" ".join(details),
            policy_mode=(
                cast("Literal['legacy', 'off', 'fixed', 'mapped'] | None", feedback["policy_mode"])
                if policy_valid
                else None
            ),
            actual_realtime_bpm=_actual_realtime_bpm(feedback) if policy_valid else None,
            guidance=(
                "refused" if refused else adoption.guidance if adoption is not None else "pending"
            ),
            map_readiness=readiness,
        )
