"""Independent global timing intent over the existing complete runtime publication."""

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from flitzis_looper.timing_policy import next_global_timing_policy

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper.controller.transport.source_sync import SourceSyncStatus

type TimingControl = Literal["sync", "realtime_bpm"]


@dataclass(frozen=True, slots=True, kw_only=True)
class GlobalTimingControlStatus:
    """Requested choice, inherited preference and actual audio stay distinct."""

    requested: bool | None
    preference: bool | None
    effective: bool | None
    pending: bool
    detail: str | None = None
    available: bool = True
    grid_phase_confirmed: bool | None = None


class TimingPolicyController:
    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project

    def set_enabled(self, control: TimingControl, *, enabled: bool) -> None:
        """Persist one checked global revision without rewriting any content intent."""
        control = "sync"  # Historical REALTIME MIDI bindings address the single SYNC control.
        current = self._project.global_timing_policy
        updated = next_global_timing_policy(current, control=control, enabled=enabled)
        if updated == current:
            return
        if not self.available(control):
            message = "Timing control requires its available requested parents."
            raise ValueError(message)
        self._project.global_timing_policy = updated
        self._transport._mark_project_changed()
        # A reserved cohort may fence publication. Intent remains requested;
        # the existing runtime tick retries after its actual ACK and mirror.
        self._transport.refresh_input_runtime()

    def available(self, control: TimingControl) -> bool:
        """Configuration uses requested parents; callback mode/phase ACK is unrelated."""
        return self._project.resolve_bpm_lock()

    def toggle(self, control: TimingControl) -> None:
        """Use the same resolved preference for direct GUI and learned actions."""
        preference = self.preference(control)
        self.set_enabled(control, enabled=preference is not True)

    def preference(self, control: TimingControl) -> bool | None:
        policy = self._project.global_timing_policy
        if policy.sync is not None:
            return policy.sync
        preferences = {
            self._project.pad_source_sync_preference(sample_id)
            for sample_id, path in enumerate(self._project.sample_paths)
            if path is not None
        }
        return next(iter(preferences)) if len(preferences) == 1 else None if preferences else False

    def status(self, control: TimingControl) -> GlobalTimingControlStatus:
        """Aggregate real PAD observations; a global request cannot borrow old ACKs."""
        policy = self._project.global_timing_policy
        states = [
            state
            for sample_id, path in enumerate(self._project.sample_paths)
            if path is not None
            and self._participates(
                sample_id, state := self._transport.source_sync.status(sample_id)
            )
        ]
        values = {state.effective for state in states}
        pending = any(state.state in {"pending", "refused", "unconfirmed"} for state in states)
        detail = next(
            (
                state.guidance_text
                for guidance in (
                    "refused",
                    "unconfirmed",
                    "pending",
                    "settling",
                    "phase",
                    "restart",
                )
                for state in states
                if state.guidance == guidance
            ),
            None,
        )
        phase_states = [state for state in states if state.requested]
        phase_confirmed = bool(phase_states) and all(
            state.active is True and state.grid_phase_confirmed is True for state in phase_states
        )
        return GlobalTimingControlStatus(
            requested=policy.sync,
            preference=self.preference(control),
            effective=next(iter(values)) if len(values) == 1 else None,
            pending=pending,
            detail=detail,
            available=self.available(control),
            grid_phase_confirmed=True if phase_confirmed else None,
        )

    def _participates(self, sample_id: int, state: SourceSyncStatus) -> bool:
        if state.active is True or self._launch_pending(sample_id) is not False:
            return True
        return state.active is None and not self._proven_inactive(sample_id)

    def _launch_pending(self, sample_id: int) -> bool | None:
        try:
            admitted = self._transport._audio.admitted_launch_ids()
        except RuntimeError, ValueError, TypeError, AttributeError:
            return None
        if not isinstance(admitted, list):
            return None
        return sample_id in admitted

    def _proven_inactive(self, sample_id: int) -> bool:
        """Exclude a stopped source only from a fresh, unfenced Native observation."""
        content = self._project.pad_content[sample_id]
        fence = self._transport.playback._pause_intent._is_observation_fenced
        if content is None or fence(sample_id):
            return False
        try:
            if self._launch_pending(sample_id) is not False:
                return False
            observation = self._transport._audio._current_ordinary_transport_observation(sample_id)
            if self._launch_pending(sample_id) is not False:
                return False
        except RuntimeError, ValueError, TypeError, AttributeError:
            return False
        return (
            self._project.pad_content[sample_id] is content
            and not fence(sample_id)
            and observation is not None
            and observation.sample_id == sample_id
            and observation.active is False
        )
