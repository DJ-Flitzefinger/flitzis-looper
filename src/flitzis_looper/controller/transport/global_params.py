import math
from dataclasses import dataclass, replace
from string import hexdigits
from typing import TYPE_CHECKING, Literal, cast

from flitzis_looper.constants import (
    PITCH_BPM_STEP,
    SPEED_MAX,
    SPEED_MIN,
    SPEED_STEP,
    VOLUME_MAX,
    VOLUME_MIN,
)
from flitzis_looper.controller.current_binding import capture_current_pad_binding
from flitzis_looper.controller.current_timing import (
    UNRESOLVED_TIMING,
    CurrentPadTiming,
    UnresolvedTiming,
)
from flitzis_looper.controller.transport.key_lock_status import KeyLockStatus
from flitzis_looper.controller.validation import ensure_finite, normalize_bpm
from flitzis_looper.models import (
    LEGACY_TRIGGER_QUANTIZATION_TO_STEP,
    TRIGGER_QUANTIZATION_STEPS,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper.models import (
        TriggerQuantizationMode,
        TriggerQuantizationStep,
    )
    from flitzis_looper_audio import BpmLockStatus, InputRuntimePadBinding


@dataclass(frozen=True, slots=True)
class GlobalRateMasterProvenance:
    """Immutable actual-master correlation, never an audio publication permit."""

    kind: Literal["absent", "unknown", "qualified"]
    source_identity: str | None = None
    accepted_revision: str | None = None


@dataclass(frozen=True, slots=True)
class GlobalRateStatus:
    """Separate requested settings from one coherent callback-owned rate observation."""

    requested_speed: float
    request_revision: int | None = None
    effect_revision: int | None = None
    state: Literal["queued", "preparing", "applied", "rejected"] | None = None
    requested_master_period: float | None = None
    effective_speed: float | None = None
    effective_master_period: float | None = None
    execution_frame: int | None = None
    error: str | None = None
    master_provenance: GlobalRateMasterProvenance | None = None


@dataclass(frozen=True, slots=True)
class BpmLockApplicationStatus:
    """Keep a remembered BPM wish separate from Native admission and actual effect."""

    requested: bool
    effective: bool | None = None
    pending: bool = True
    detail: str | None = None


@dataclass(slots=True)
class _BpmLockApplication:
    enabled: bool
    original_content_id: str | None
    fingerprint: tuple[object, ...] | None = None
    admitted_revision: int | None = None
    attempted: bool = False
    master_projected: bool = False
    unobserved_after_revision: int | None = None


def _decode_bpm_lock_status(feedback: object) -> BpmLockStatus:
    """Validate one coherent Native receipt without guessing effect from enqueue."""
    if not isinstance(feedback, dict):
        message = "BPM Lock confirmation is unavailable."
        raise TypeError(message)
    required = {
        "request_revision",
        "effect_revision",
        "state",
        "requested_enabled",
        "effective_enabled",
        "reason",
        "execution_frame",
        "submission_available",
    }
    if not required.issubset(feedback):
        message = "BPM Lock confirmation has an incomplete receipt."
        raise ValueError(message)
    for key in ("request_revision", "effect_revision"):
        if type(feedback[key]) is not int or not 0 <= feedback[key] < 1 << 64:
            message = "BPM Lock confirmation has an invalid receipt revision."
            raise ValueError(message)
    if feedback["effect_revision"] > feedback["request_revision"]:
        message = "BPM Lock confirmation has inconsistent receipt revisions."
        raise ValueError(message)
    _validate_bpm_status_flags(cast("BpmLockStatus", feedback))
    frame = feedback["execution_frame"]
    if frame is not None and (type(frame) is not int or not 0 <= frame < 1 << 64):
        message = "BPM Lock confirmation has an invalid execution frame."
        raise ValueError(message)
    initial = (
        feedback["request_revision"] == feedback["effect_revision"] == 0
        and feedback["requested_enabled"] is False
        and feedback["effective_enabled"] is False
    )
    invalid_effect = (
        feedback["effect_revision"] != feedback["request_revision"]
        or feedback["reason"] is not None
        or feedback["requested_enabled"] is not feedback["effective_enabled"]
        or (frame is None and not initial)
    )
    if feedback["state"] == "applied" and invalid_effect:
        message = "BPM Lock confirmation has an inconsistent applied receipt."
        raise ValueError(message)
    return cast("BpmLockStatus", feedback)


def _validate_bpm_status_flags(feedback: BpmLockStatus) -> None:
    values = cast("dict[str, object]", feedback)
    if feedback["state"] not in {"queued", "pending", "applied"}:
        message = "BPM Lock confirmation has an invalid state."
        raise ValueError(message)
    for key in ("requested_enabled", "effective_enabled", "submission_available"):
        if type(values[key]) is not bool:
            message = "BPM Lock confirmation has an invalid boolean."
            raise ValueError(message)
    if feedback["reason"] not in {
        None,
        "mapped_active",
        "rate_pending",
        "drain_pending",
        "history_pending",
        "policy_pending",
        "reference_changed",
    }:
        message = "BPM Lock confirmation has an invalid pending reason."
        raise ValueError(message)


def _rate_number(value: object, *, optional: bool = False) -> float | None:
    if value is None and optional:
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        message = "Native rate confirmation has an invalid numeric setting."
        raise TypeError(message)
    try:
        number = float(value)
    except OverflowError as error:
        message = "Native rate confirmation has an invalid numeric setting."
        raise ValueError(message) from error
    if not math.isfinite(number) or number <= 0.0:
        message = "Native rate confirmation has an invalid numeric setting."
        raise ValueError(message)
    return number


def _rate_revision(value: object) -> int:
    if type(value) is not int:
        message = "Native rate confirmation has an invalid receipt revision."
        raise TypeError(message)
    if value < 0:
        message = "Native rate confirmation has an invalid receipt revision."
        raise ValueError(message)
    return value


def _rate_state(
    value: object, *, request: int, effect: int
) -> Literal["queued", "preparing", "applied", "rejected"]:
    if not isinstance(value, str):
        message = "Native rate confirmation has an invalid receipt state."
        raise TypeError(message)
    if value not in {"queued", "preparing", "applied", "rejected"}:
        message = "Native rate confirmation has an invalid receipt state."
        raise ValueError(message)
    if (value == "queued") != (effect != request):
        message = "Native rate confirmation has inconsistent receipt revisions."
        raise ValueError(message)
    return cast('Literal["queued", "preparing", "applied", "rejected"]', value)


def _rate_error(value: object, *, state: str) -> str | None:
    if value is not None and not isinstance(value, str):
        message = "Native rate confirmation has an invalid receipt error."
        raise TypeError(message)
    if (state == "rejected") != (value is not None) or (value is not None and not value):
        message = "Native rate confirmation has an inconsistent refusal reason."
        raise ValueError(message)
    return value


def _decode_master_provenance(
    value: object, *, effective_period: float | None
) -> GlobalRateMasterProvenance:
    if not isinstance(value, dict) or not {"kind", "source_identity", "accepted_revision"}.issubset(
        value
    ):
        message = "Native rate confirmation has incomplete master provenance."
        raise ValueError(message)
    kind = value["kind"]
    if not isinstance(kind, str) or kind not in {"absent", "unknown", "qualified"}:
        message = "Native rate confirmation has invalid master provenance."
        raise ValueError(message)
    source = value["source_identity"]
    revision = value["accepted_revision"]
    if kind != "qualified":
        if source is not None or revision is not None:
            message = "Native rate confirmation has inconsistent master provenance."
            raise ValueError(message)
        return GlobalRateMasterProvenance(cast('Literal["absent", "unknown"]', kind))
    valid_source = (
        isinstance(source, str)
        and len(source) == 64
        and all(character in hexdigits for character in source)
    )
    valid_revision = revision is None or (isinstance(revision, str) and bool(revision))
    if not valid_source or not valid_revision or effective_period is None:
        message = "Native rate confirmation has invalid qualified master provenance."
        raise ValueError(message)
    return GlobalRateMasterProvenance("qualified", source, revision)


def _decode_rate_status(feedback: object, *, minimum_revision: int) -> GlobalRateStatus:
    if not isinstance(feedback, dict):
        message = "Native rate confirmation is unavailable."
        raise TypeError(message)
    values = cast("dict[str, object]", feedback)
    required = {
        "request_revision",
        "effect_revision",
        "state",
        "requested_speed",
        "requested_master_period",
        "effective_speed",
        "effective_master_period",
        "execution_frame",
        "error",
        "master_provenance",
    }
    if not required.issubset(values):
        message = "Native rate confirmation has an incomplete receipt."
        raise ValueError(message)
    request = _rate_revision(values["request_revision"])
    effect = _rate_revision(values["effect_revision"])
    if request < minimum_revision:
        message = "Native rate confirmation returned an older request receipt."
        raise ValueError(message)
    if effect > request:
        message = "Native rate confirmation has an invalid receipt."
        raise ValueError(message)
    state = _rate_state(values["state"], request=request, effect=effect)
    frame_value = values["execution_frame"]
    frame = None if frame_value is None else _rate_revision(frame_value)
    if state == "queued" and frame is not None:
        message = "Native rate confirmation has an unconfirmed execution frame."
        raise ValueError(message)
    requested = _rate_number(values["requested_speed"])
    assert requested is not None
    effective_period = _rate_number(values["effective_master_period"], optional=True)
    return GlobalRateStatus(
        requested_speed=requested,
        request_revision=request,
        effect_revision=effect,
        state=state,
        requested_master_period=_rate_number(values["requested_master_period"], optional=True),
        effective_speed=_rate_number(values["effective_speed"]),
        effective_master_period=effective_period,
        execution_frame=frame,
        error=_rate_error(values["error"], state=state),
        master_provenance=_decode_master_provenance(
            values["master_provenance"], effective_period=effective_period
        ),
    )


class GlobalParametersController:
    """Manage global playback modes/states (multi-loop, key lock, BPM lock, etc.)."""

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._session = transport._session
        self._audio = transport._audio
        self._bpm = transport.bpm
        self._rate_status: GlobalRateStatus | None = None
        self._rate_status_error: str | None = None
        self._rate_request_unobserved = False
        self._rate_observed_revision = 0
        self._rate_request_after_revision = 0
        self._bpm_application: _BpmLockApplication | None = None
        self._bpm_feedback: BpmLockStatus | None = None
        self._bpm_application_error: str | None = None

    def _poll_global_rate_status(
        self,
        *,
        qualified: tuple[InputRuntimePadBinding, tuple[object, ...]] | None = None,
    ) -> bool:
        """Observe Native once on the normal runtime tick without publishing control intent."""
        observe = getattr(self._audio, "global_rate_status", None)
        self._rate_status = None
        self._rate_status_error = None
        if not callable(observe):
            self._rate_status_error = "Rate confirmation requires the current audio runtime."
            return False
        try:
            feedback = observe()
            if feedback is None:
                return False
            status = _decode_rate_status(feedback, minimum_revision=self._rate_observed_revision)
            revision = status.request_revision
            assert revision is not None
            self._rate_status = status
            self._rate_observed_revision = revision
            mirrored = False
            if self._bpm_feedback is not None and self._bpm_feedback["effective_enabled"]:
                mirrored = self._mirror_actual_global_rate(status, qualified=qualified)
            if revision > self._rate_request_after_revision:
                self._rate_request_unobserved = False
        except (RuntimeError, ValueError, TypeError) as error:
            self._rate_status_error = str(error)
            return False
        else:
            return mirrored

    def global_rate_status(self) -> GlobalRateStatus:
        """Return cached audio confirmation independently of the persisted speed control."""
        status = self._rate_status
        if status is None:
            return GlobalRateStatus(
                requested_speed=float(self._project.speed), error=self._rate_status_error
            )
        if self._rate_request_unobserved:
            return replace(
                status,
                requested_speed=float(self._project.speed),
                request_revision=None,
                state="queued",
                requested_master_period=None,
                execution_frame=None,
                error=None,
            )
        return status

    def _accepted_refresh_speed(self) -> float | None:
        """Wait for a terminal coherent rate image without adopting its requested speed."""
        status = self.global_rate_status()
        if status.state not in {"applied", "rejected"}:
            return None
        speed = status.effective_speed
        return speed if speed is not None and math.isfinite(speed) and speed > 0.0 else None

    def set_multi_loop(self, *, enabled: bool) -> None:
        """Enable or disable Multi Loop mode."""
        if type(enabled) is not bool:
            message = "Multi Loop requires a strict boolean."
            raise TypeError(message)
        if self._project.multi_loop is enabled:
            return
        self._transport.stage_multi_loop_cancellation(enabled=enabled)
        self._project.multi_loop = enabled
        self._begin_bpm_lock_application(capture_anchor=enabled and self._project.bpm_lock)
        self._transport._mark_project_changed()
        self.poll_bpm_lock_application()

    def set_key_lock(self, *, enabled: bool) -> None:
        """Change the default and publish only pads that inherit it."""
        self._transport.ensure_global_key_lock_edit_allowed()
        generations = self._capture_inherited_generations()
        changed = enabled != self._project.key_lock
        self._project.key_lock = enabled
        if self._transport._on_global_key_lock_changed is not None:
            self._transport._on_global_key_lock_changed()

        for sample_id in range(len(self._project.pad_key_lock)):
            if self._project.sample_paths[sample_id] is None:
                if self._project.pad_key_lock[sample_id] is not None:
                    self._project.pad_key_lock[sample_id] = None
                    changed = True
                continue
            if self._project.pad_key_lock[sample_id] is not None or sample_id not in generations:
                continue

            try:
                self._transport.residency.apply_key_lock(
                    sample_id, expected_source_generation=generations[sample_id]
                )
            except (RuntimeError, ValueError) as error:
                self._transport.residency.report_key_lock_error(sample_id, str(error))
            changed = True

        if not changed:
            return

        self._transport._mark_project_changed()

    def _capture_inherited_generations(self) -> dict[int, int | None]:
        generations: dict[int, int | None] = {}
        for sample_id, path in enumerate(self._project.sample_paths):
            if path is None or self._project.pad_key_lock[sample_id] is not None:
                continue
            provider = self._transport._incoming_load_request
            if provider is not None and provider(sample_id) is not None:
                # Only the request-bound resolver owns incoming Global inheritance.
                continue
            try:
                generations[sample_id] = self._transport.capture_current_pad_edit_generation(
                    sample_id
                )
            except (RuntimeError, ValueError) as error:
                self._transport.residency.report_key_lock_error(sample_id, str(error))
                continue
            self._transport.residency.remember(sample_id)
        return generations

    def key_lock_status(self) -> KeyLockStatus:
        """Summarize current per-target confirmations across all banks without rolling them back."""
        states = [
            self._transport.residency.key_lock_status(sample_id)
            for sample_id, path in enumerate(self._project.sample_paths)
            if path is not None
        ]
        modes = {state.effective for state in states}
        effective = next(iter(modes)) if len(modes) == 1 else None
        errors = [state.error for state in states if state.error]
        return KeyLockStatus(
            self._project.key_lock,
            effective,
            pending=any(state.pending for state in states),
            unconfirmed=any(state.unconfirmed for state in states),
            error=f"{len(errors)} pad(s): {errors[0]}" if errors else None,
            mixed=len(modes) > 1,
        )

    def set_bpm_lock(self, *, enabled: bool) -> None:
        """Enable or disable BPM Lock mode."""
        if type(enabled) is not bool:
            message = "BPM Lock requires a strict boolean."
            raise TypeError(message)
        if enabled == self._project.bpm_lock:
            return
        if not self._project.multi_loop:
            message = "BPM Lock requires available Multi Loop timing."
            raise ValueError(message)
        if enabled:
            self._transport.ensure_pad_edit_allowed(self._project.selected_pad)
        self._project.bpm_lock = enabled
        self._begin_bpm_lock_application(capture_anchor=enabled)
        self._transport._mark_project_changed()
        self.poll_bpm_lock_application()

    def _begin_bpm_lock_application(self, *, capture_anchor: bool) -> None:
        """Capture placement once; Pair's existing mirror is its only later mover."""
        enabled = self._project.resolve_bpm_lock()
        if capture_anchor and enabled:
            self._session.bpm_lock_anchor_pad_id = self._project.selected_pad
            self._session.bpm_lock_anchor_bpm = None
            self._session.bpm_lock_anchor_revision = None
        self._bpm_application = _BpmLockApplication(enabled, None)
        self._bpm_application_error = None
        if enabled:
            try:
                self._bind_first_bpm_content()
            except (RuntimeError, ValueError) as error:
                self._bpm_application_error = str(error)

    def _bpm_lock_application_status(self) -> BpmLockApplicationStatus:
        """Return cached actual effect, preserving pending and missing-runtime errors."""
        feedback = self._bpm_feedback
        application = self._bpm_application
        effective = None if feedback is None else feedback["effective_enabled"] is True
        applied = self._bpm_receipt_applied()
        current = (
            application is None
            or not application.enabled
            or self._own_actual_master_provenance(self._rate_status) is not None
        )
        return BpmLockApplicationStatus(
            requested=self._project.bpm_lock,
            effective=effective,
            pending=not applied or not current or self._bpm_application_error is not None,
            detail=self._bpm_application_error
            or (str(feedback["reason"]) if feedback is not None and feedback["reason"] else None)
            or ("BPM Lock is waiting for actual master confirmation." if not current else None),
        )

    def _bpm_receipt_applied(self) -> bool:
        application = self._bpm_application
        feedback = self._bpm_feedback
        return (
            application is not None
            and feedback is not None
            and application.admitted_revision == feedback["request_revision"]
            and feedback["state"] == "applied"
            and feedback["requested_enabled"] is application.enabled
        )

    def _bpm_lock_master_ready(self) -> bool:
        """Check the historical receipt and original owner without resolving another clock."""
        application = self._bpm_application
        if application is None or not application.enabled or not self._project.resolve_bpm_lock():
            return False
        anchor = self._session.bpm_lock_anchor_pad_id
        content = self._project.pad_content[anchor] if anchor is not None else None
        return (
            self._bpm_receipt_applied()
            and content is not None
            and content.instance_id == application.original_content_id
        )

    def _capture_bpm_master_binding(
        self, *, timing: CurrentPadTiming | None = None
    ) -> InputRuntimePadBinding | None:
        """Capture original-source authority for one derived Native master effect."""
        if timing is None or not self._bpm_lock_master_ready():
            return None
        qualified = self._qualified_bpm_anchor(timing=timing)
        if qualified is None:
            return None
        return qualified[0]

    def _qualified_bpm_anchor(
        self, *, timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING
    ) -> tuple[InputRuntimePadBinding, tuple[object, ...]] | None:
        application = self._bpm_application
        anchor = self._session.bpm_lock_anchor_pad_id
        if application is None or anchor is None or application.original_content_id is None:
            return None
        content = self._project.pad_content[anchor]
        if content is None or content.instance_id != application.original_content_id:
            return None
        self._transport.ensure_current_source_assignment_ready(anchor)
        if isinstance(timing, UnresolvedTiming):
            timing = self._bpm.current_timing(anchor)
        if timing is None:
            return None
        snapshot = capture_current_pad_binding(self._audio, anchor, timing=timing)
        if snapshot is None:
            return None
        # Placement and constant-timing authority rebasing are not new source or
        # accepted evidence. Pair owns placement; the original content and current
        # source/window/accepted timing still qualify every musical master effect.
        signature = snapshot.source_signature
        return snapshot.binding, (signature[1:8], signature[9:], timing)

    def _bpm_anchor_waiting_for_first_content(self, sample_id: int | None) -> bool:
        """An unsubmitted empty activation keeps its one captured slot until first binding."""
        if (
            sample_id is None
            or sample_id != self._session.bpm_lock_anchor_pad_id
            or not self._project.resolve_bpm_lock()
        ):
            return False
        application = self._bpm_application
        if (
            application is None
            or not application.enabled
            or application.original_content_id is not None
        ):
            return False
        return (
            application.admitted_revision is None
            and not application.attempted
            and application.unobserved_after_revision is None
        )

    def _bind_first_bpm_content(self) -> None:
        anchor = self._session.bpm_lock_anchor_pad_id
        if not self._bpm_anchor_waiting_for_first_content(anchor):
            return
        assert anchor is not None
        self._transport.ensure_current_source_assignment_ready(anchor)
        content = self._project.pad_content[anchor]
        if content is not None:
            application = self._bpm_application
            assert application is not None
            application.original_content_id = content.instance_id

    def poll_bpm_lock_application(self) -> None:
        """Qualify unadmitted work once per fresh coherent observation; Native owns retries."""
        observe = getattr(self._audio, "bpm_lock_status", None)
        if not callable(observe):
            self._bpm_application_error = (
                "BPM Lock confirmation requires the current audio runtime."
            )
            return
        try:
            feedback = _decode_bpm_lock_status(observe())
        except (RuntimeError, ValueError, TypeError) as error:
            self._bpm_application_error = str(error)
            return
        self._bpm_feedback = feedback
        application = self._bpm_application
        if application is None:
            return
        if application.enabled != self._project.resolve_bpm_lock():
            self._begin_bpm_lock_application(capture_anchor=False)
            application = self._bpm_application
            assert application is not None
        if self._observe_unconfirmed_bpm_admission(application, feedback):
            self._service_bpm_application(application, feedback, observe)

    def _service_bpm_application(
        self,
        application: _BpmLockApplication,
        feedback: BpmLockStatus,
        observe: Callable[[], object],
    ) -> None:
        try:
            self._bind_first_bpm_content()
            self._transport.refresh_input_runtime()
            qualified = self._qualified_bpm_anchor() if application.enabled else None
            if application.enabled and qualified is None:
                self._bpm_application_error = "BPM Lock is waiting for its original source timing."
                return
            if not self._bpm_application_needs_admission(application, feedback, qualified):
                if self._bpm_receipt_applied() and application.master_projected:
                    # A fresh current-source observation may recover from a
                    # transient read/Hold error without replaying an audio effect.
                    self._bpm_application_error = None
                if feedback["state"] == "applied" and feedback["submission_available"]:
                    self._reconcile_manual_bpm_master(qualified)
                return
            if application.attempted and feedback["submission_available"] is not True:
                return
            self._submit_bpm_application(application, feedback, qualified, observe)
        except (RuntimeError, ValueError, TypeError) as error:
            self._bpm_application_error = str(error)

    def _reconcile_manual_bpm_master(
        self, qualified: tuple[InputRuntimePadBinding, tuple[object, ...]] | None
    ) -> None:
        if qualified is None:
            return
        timing = qualified[1][-1]
        if isinstance(timing, CurrentPadTiming):
            self._publish_manual_bpm_master(timing=timing, periodic=True)

    def _manual_master_target(
        self, timing: CurrentPadTiming
    ) -> (
        tuple[
            _BpmLockApplication,
            tuple[InputRuntimePadBinding, tuple[object, ...]],
            float,
            GlobalRateStatus,
        ]
        | None
    ):
        if timing.accepted_revision is not None:
            return None
        application = self._bpm_application
        if application is None or not self._project.resolve_bpm_lock():
            return None
        if not application.enabled or not self._bpm_receipt_applied():
            return None
        qualified = self._qualified_bpm_anchor(timing=timing)
        if qualified is None or qualified[1][-1] != timing:
            return None
        speed = self._accepted_refresh_speed()
        if speed is None:
            return None
        return application, qualified, timing.period_seconds / speed, self.global_rate_status()

    def _publish_manual_bpm_master(
        self, *, timing: CurrentPadTiming, periodic: bool = False
    ) -> bool:
        """Reconcile current manual timing against actual speed without another pending owner."""
        target = self._manual_master_target(timing)
        if target is None:
            return False
        application, qualified, period, status = target
        provenance = self._own_actual_master_provenance(status, qualified=qualified)
        if (
            status.effective_master_period == period
            and provenance is not None
            and provenance.accepted_revision is None
        ):
            return False
        same_refusal = (
            periodic
            and status.state == "rejected"
            and status.requested_master_period == period
            and application.fingerprint == qualified[1]
        )
        if same_refusal:
            return False
        self._audio.set_master_period(period, anchor_binding=qualified[0])
        application.fingerprint = qualified[1]
        self._record_rate_request()
        self._bpm_application_error = None
        return True

    @staticmethod
    def _observe_unconfirmed_bpm_admission(
        application: _BpmLockApplication, feedback: BpmLockStatus
    ) -> bool:
        """A normal setter return retains ownership even while the receipt is unreadable."""
        if application.unobserved_after_revision is not None:
            if (
                feedback["request_revision"] > application.unobserved_after_revision
                and feedback["requested_enabled"] is application.enabled
            ):
                application.admitted_revision = feedback["request_revision"]
                application.unobserved_after_revision = None
            else:
                return False
        return True

    def _bpm_application_needs_admission(
        self,
        application: _BpmLockApplication,
        feedback: BpmLockStatus,
        qualified: tuple[InputRuntimePadBinding, tuple[object, ...]] | None,
    ) -> bool:
        if application.admitted_revision is None:
            return True
        if feedback["request_revision"] != application.admitted_revision:
            return False
        fingerprint_changed = (
            application.enabled
            and qualified is not None
            and qualified[1] != application.fingerprint
            and feedback["state"] != "applied"
        )
        if (
            feedback["state"] == "applied"
            and not application.master_projected
            and not fingerprint_changed
        ):
            self._mirror_bpm_application_effect(application, qualified=qualified)
        if (feedback["reason"] == "reference_changed" or fingerprint_changed) and (
            not application.enabled or qualified is not None
        ):
            # The stale receipt is meaningful even when only hierarchy changed.
            application.admitted_revision = None
            application.attempted = False
            application.master_projected = False
            return True
        return False

    def _mirror_bpm_application_effect(
        self,
        application: _BpmLockApplication,
        *,
        qualified: tuple[InputRuntimePadBinding, tuple[object, ...]] | None,
    ) -> None:
        """Mirror Native's qualified effect; flag ACK never queues another master/bootstrap."""
        if application.enabled:
            # Read after the matching flag ACK: the preceding tick's rate cache
            # may predate Native's atomic BPM/master/bootstrap application.
            if not self._poll_global_rate_status(qualified=qualified):
                return
        else:
            self._session.master_bpm = None
            self._session.master_period_seconds = None
            if not self._project.bpm_lock:
                self._session.bpm_lock_anchor_pad_id = None
                self._session.bpm_lock_anchor_bpm = None
                self._session.bpm_lock_anchor_revision = None
        application.master_projected = True
        self._bpm_application_error = None

    def _own_actual_master_provenance(
        self,
        status: GlobalRateStatus | None,
        *,
        qualified: tuple[InputRuntimePadBinding, tuple[object, ...]] | None = None,
    ) -> GlobalRateMasterProvenance | None:
        """Correlate actual proof with today's original source without granting authority."""
        if status is None or not self._bpm_lock_master_ready():
            return None
        provenance = status.master_provenance
        if provenance is None or provenance.kind != "qualified":
            return None
        try:
            if qualified is None:
                qualified = self._qualified_bpm_anchor()
            if qualified is None or qualified[0].is_current() is not True:
                return None
            source = qualified[0].metadata().get("source_identity")
        except RuntimeError, ValueError, TypeError, KeyError:
            # Temporary Hold/read failures invalidate only this derived view.
            return None
        return (
            provenance if isinstance(source, str) and source == provenance.source_identity else None
        )

    def _mirror_actual_global_rate(
        self,
        status: GlobalRateStatus,
        *,
        qualified: tuple[InputRuntimePadBinding, tuple[object, ...]] | None = None,
    ) -> bool:
        provenance = self._own_actual_master_provenance(status, qualified=qualified)
        period = status.effective_master_period
        speed = status.effective_speed
        if provenance is None or period is None or speed is None:
            return False
        self._session.master_period_seconds = period
        self._session.master_bpm = 60.0 / period
        self._session.bpm_lock_anchor_bpm = 60.0 / (period * speed)
        self._session.bpm_lock_anchor_revision = provenance.accepted_revision
        return True

    def _record_rate_request(self) -> None:
        self._rate_request_unobserved = True
        self._rate_request_after_revision = self._rate_observed_revision

    def _submit_bpm_application(
        self,
        application: _BpmLockApplication,
        feedback: BpmLockStatus,
        qualified: tuple[InputRuntimePadBinding, tuple[object, ...]] | None,
        observe: Callable[[], object],
    ) -> None:
        application.attempted = True
        before = feedback["request_revision"]
        if application.enabled:
            assert qualified is not None
            self._audio.set_bpm_lock(enabled=True, anchor_binding=qualified[0])
            application.fingerprint = qualified[1]
        else:
            self._audio.set_bpm_lock(enabled=False)
        application.unobserved_after_revision = before
        admitted = _decode_bpm_lock_status(observe())
        self._bpm_feedback = admitted
        if (
            admitted["request_revision"] > before
            and admitted["requested_enabled"] is application.enabled
        ):
            application.admitted_revision = admitted["request_revision"]
            application.unobserved_after_revision = None
            self._bpm_application_error = None
        else:
            self._bpm_application_error = "BPM Lock admission is pending."

    def _publish_trigger_quantization(
        self, *, enabled: bool, step: TriggerQuantizationStep
    ) -> None:
        self._audio.set_trigger_quantization(step if enabled else "immediate")

    def set_trigger_quantization_enabled(self, *, enabled: bool) -> None:
        """Enable or disable global trigger quantization."""
        if enabled == self._project.trigger_quantization_enabled:
            return

        self._publish_trigger_quantization(
            enabled=enabled, step=self._project.trigger_quantization_step
        )
        self._project.trigger_quantization_enabled = enabled
        self._transport._mark_project_changed()

    def toggle_trigger_quantization(self) -> None:
        """Toggle global trigger quantization on or off."""
        self.set_trigger_quantization_enabled(
            enabled=not self._project.trigger_quantization_enabled
        )

    def set_trigger_quantization_step(self, step: TriggerQuantizationStep) -> None:
        """Set the global trigger quantization grid step."""
        if step == self._project.trigger_quantization_step:
            return

        if self._project.trigger_quantization_enabled:
            self._publish_trigger_quantization(enabled=True, step=step)
        self._project.trigger_quantization_step = step
        self._transport._mark_project_changed()

    def set_trigger_quantization(self, mode: TriggerQuantizationMode | str) -> None:
        """Set global trigger quantization from legacy mode strings."""
        if mode in {"immediate", "disabled", "off"}:
            self.set_trigger_quantization_enabled(enabled=False)
            return

        if mode == "enabled":
            self.set_trigger_quantization_enabled(enabled=True)
            return

        step = LEGACY_TRIGGER_QUANTIZATION_TO_STEP.get(str(mode))
        if step is None:
            if mode not in TRIGGER_QUANTIZATION_STEPS:
                msg = "trigger quantization mode is unsupported"
                raise ValueError(msg)
            step = cast("TriggerQuantizationStep", mode)

        changed = (
            not self._project.trigger_quantization_enabled
            or step != self._project.trigger_quantization_step
        )
        if not changed:
            return

        self._publish_trigger_quantization(enabled=True, step=step)
        self._project.trigger_quantization_step = step
        self._project.trigger_quantization_enabled = True
        self._transport._mark_project_changed()

    def set_volume(self, volume: float) -> None:
        """Set global volume."""
        ensure_finite(volume)
        clamped = min(max(volume, VOLUME_MIN), VOLUME_MAX)
        if not self._session.global_stop_momentary_mute_active:
            self._audio.set_volume(clamped)
        self._project.volume = clamped
        self._transport._mark_project_changed()

    def set_momentary_output_mute(self, *, enabled: bool) -> None:
        """Temporarily mute engine output without changing persisted volume."""
        if enabled:
            if self._session.global_stop_momentary_mute_active:
                return
            self._audio.set_volume(VOLUME_MIN)
            self._session.global_stop_momentary_mute_active = True
            return

        if not self._session.global_stop_momentary_mute_active:
            return
        self._audio.set_volume(self._project.volume)
        self._session.global_stop_momentary_mute_active = False

    def set_speed(
        self,
        speed: float,
        *,
        anchor_timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> None:
        """Set global playback speed multiplier."""
        ensure_finite(speed)
        qualified_timing = self._speed_anchor_timing(anchor_timing)
        clamped = min(max(speed, SPEED_MIN), SPEED_MAX)
        if qualified_timing is not None:
            binding = self._capture_bpm_master_binding(timing=qualified_timing)
            if binding is None:
                message = "The original BPM reference changed before the speed request."
                raise RuntimeError(message)
            self._audio.set_speed_and_master_period(
                clamped, qualified_timing.period_seconds / clamped, anchor_binding=binding
            )
        else:
            self._audio.set_speed(clamped)
        self._record_rate_request()
        self._project.speed = clamped
        self._transport._mark_project_changed()
        self._bpm.recompute_master_bpm(
            timing=anchor_timing, publish_master=qualified_timing is None
        )

    def _speed_anchor_timing(
        self, timing: CurrentPadTiming | UnresolvedTiming | None
    ) -> CurrentPadTiming | None:
        master_ready = self._bpm_lock_master_ready()
        actual_locked = self._bpm_feedback is not None and self._bpm_feedback["effective_enabled"]
        if (self._project.resolve_bpm_lock() or actual_locked) and not master_ready:
            message = "The original BPM reference is not acknowledged for the speed request."
            raise RuntimeError(message)
        if not master_ready:
            return None
        anchor_id = self._session.bpm_lock_anchor_pad_id
        assert anchor_id is not None
        self._transport.ensure_pad_edit_allowed(anchor_id)
        if isinstance(timing, UnresolvedTiming):
            timing = self._bpm.current_timing(anchor_id)
        if timing is None:
            message = "current anchor timing is not acknowledged"
            raise RuntimeError(message)
        return timing

    def speed_reference_bpm(self) -> float | None:
        """Return the BPM value represented by 1.00x speed for the current context."""
        period = self._speed_reference_period_seconds()
        return 60.0 / period if period is not None else None

    def _speed_reference_period_seconds(self) -> float | None:
        """Return the current source quarter period for the global speed control."""
        period, _timing = self._speed_reference()
        return period

    def _speed_reference(
        self,
    ) -> tuple[float | None, CurrentPadTiming | UnresolvedTiming | None]:
        speed = float(self._project.speed)
        if speed <= 0.0:
            return None, UNRESOLVED_TIMING

        if self._project.resolve_bpm_lock():
            anchor_pad_id = self._session.bpm_lock_anchor_pad_id
            if anchor_pad_id is not None:
                timing = self._bpm.current_timing(anchor_pad_id)
                return (timing.period_seconds if timing is not None else None), timing
            master_bpm = normalize_bpm(self._session.master_bpm)
            if master_bpm is not None:
                return 60.0 / (float(master_bpm) / speed), UNRESOLVED_TIMING

        timing = self._bpm.current_timing(self._project.selected_pad)
        return (timing.period_seconds if timing is not None else None), UNRESOLVED_TIMING

    def effective_display_bpm(self) -> float | None:
        """Return requested BPM for the existing global Pitch target control."""
        period = self._speed_reference_period_seconds()
        if period is None:
            return None
        return 60.0 / (period / float(self._project.speed))

    def set_effective_display_bpm(self, bpm: float) -> bool:
        """Set global speed by targeting a displayed BPM value."""
        ensure_finite(bpm)
        if bpm <= 0.0:
            return False

        period, timing = self._speed_reference()
        if period is None:
            return False

        self.set_speed(period * (float(bpm) / 60.0), anchor_timing=timing)
        return True

    def nudge_speed_by_bpm_step(self, direction: int) -> None:
        """Move Pitch by one BPM-grid step, falling back to multiplier steps without BPM."""
        if direction > 0:
            self.nudge_speed_by_bpm_steps(1)
        elif direction < 0:
            self.nudge_speed_by_bpm_steps(-1)

    def nudge_speed_by_bpm_steps(self, steps: int) -> None:
        """Move Pitch by signed BPM-grid steps, or multiplier steps without BPM."""
        if steps == 0:
            return

        period, timing = self._speed_reference()
        if period is None:
            self.set_speed(float(self._project.speed) + SPEED_STEP * steps, anchor_timing=timing)
            return

        current_bpm = 60.0 / (period / float(self._project.speed))
        target_bpm = round(current_bpm + PITCH_BPM_STEP * steps, 2)
        if target_bpm > 0.0:
            self.set_speed(period * (target_bpm / 60.0), anchor_timing=timing)

    def reset_speed(self) -> None:
        """Reset global speed back to 1.0x."""
        self.set_speed(1.0)
