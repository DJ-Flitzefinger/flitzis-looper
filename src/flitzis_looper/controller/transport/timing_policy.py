"""Independent global timing intent over the existing complete runtime publication."""

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from flitzis_looper.timing_policy import next_global_timing_policy, variable_source_timing

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController

type TimingControl = Literal["sync", "realtime_bpm"]


@dataclass(frozen=True, slots=True, kw_only=True)
class GlobalTimingControlStatus:
    """Requested choice, inherited preference and actual audio stay distinct."""

    requested: bool | None
    preference: bool | None
    effective: bool | None
    pending: bool
    detail: str | None = None


class TimingPolicyController:
    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project

    def set_enabled(self, control: TimingControl, *, enabled: bool) -> None:
        """Persist one checked global revision without rewriting any content intent."""
        current = self._project.global_timing_policy
        updated = next_global_timing_policy(current, control=control, enabled=enabled)
        if updated == current:
            return
        self._project.global_timing_policy = updated
        self._transport._mark_project_changed()
        # A reserved cohort may fence publication. Intent remains requested;
        # the existing runtime tick retries after its actual ACK and mirror.
        self._transport.refresh_input_runtime()

    def toggle(self, control: TimingControl) -> None:
        """Use the same resolved preference for direct GUI and learned actions."""
        preference = self.preference(control)
        self.set_enabled(control, enabled=preference is not True)

    def preference(self, control: TimingControl) -> bool | None:
        policy = self._project.global_timing_policy
        if control == "realtime_bpm":
            return variable_source_timing(policy)
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
            self._transport.source_sync.status(sample_id)
            for sample_id, path in enumerate(self._project.sample_paths)
            if path is not None
        ]
        values = (
            {state.effective for state in states}
            if control == "sync"
            else {state.actual_realtime_bpm for state in states}
        )
        pending = any(state.state in {"pending", "refused", "unconfirmed"} for state in states)
        return GlobalTimingControlStatus(
            requested=getattr(policy, control),
            preference=self.preference(control),
            effective=next(iter(values)) if len(values) == 1 else None,
            pending=pending,
            detail="Start/retrigger applies supported timing changes." if pending else None,
        )
