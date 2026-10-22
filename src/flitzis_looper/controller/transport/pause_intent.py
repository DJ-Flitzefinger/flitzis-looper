"""Bounded requested ordinary pause intent and Native observation projection."""

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper_audio import OrdinaryTransportAdmission, OrdinaryTransportObservation


@dataclass(slots=True)
class _PauseOwner:
    monitor: OrdinaryTransportAdmission | OrdinaryTransportObservation
    admission: OrdinaryTransportAdmission | None = None
    pending: bool = False
    observed_sequence: int = 0


class OrdinaryPauseIntentController:
    """Keep one content-owned request/monitor without predicting effective audio state."""

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._session = transport._session
        self._owners: dict[str, _PauseOwner] = {}
        self._observation_fenced: Callable[[int], bool] | None = None

    def bind_observation_fence(self, fenced: Callable[[int], bool]) -> None:
        """Reuse the App's actual ordinary observer fences for durable queries as well."""
        self._observation_fenced = fenced

    def remember(self, sample_id: int, admission: OrdinaryTransportAdmission) -> None:
        """Retain the original accepted Native admission after successful enqueue."""
        if (
            type(admission.content_lifetime) is not int
            or admission.content_lifetime <= 0
            or type(admission.transport_revision) is not int
            or admission.transport_revision <= 0
            or type(admission.requested_paused) is not bool
        ):
            message = "Ordinary pause requires a current Native admission receipt"
            raise TypeError(message)
        self._prune_removed_content()
        content = self._project.pad_content[sample_id]
        if content is not None:
            self._owners[content.instance_id] = _PauseOwner(
                monitor=admission, admission=admission, pending=True
            )

    def forget_pad(self, sample_id: int) -> None:
        """Retire the original assignment owner before its content identity is removed."""
        content = self._project.pad_content[sample_id]
        if content is not None:
            self._owners.pop(content.instance_id, None)

    def requested_paused(self, sample_id: int) -> bool:
        """Project available Native truth before resolving the next ordinary toggle."""
        self.poll(sample_id=sample_id, report_unavailable=True)
        content = self._project.pad_content[sample_id]
        owner = self._owners.get(content.instance_id) if content is not None else None
        if owner is not None and owner.pending and owner.admission is not None:
            return owner.admission.requested_paused
        return sample_id in self._session.paused_sample_ids

    def poll(self, *, sample_id: int | None = None, report_unavailable: bool = False) -> None:
        """Read bounded current Native snapshots even before a content owns a receipt."""
        live_slots = self._prune_removed_content()
        if sample_id is None:
            targets = tuple(live_slots.items())
        else:
            content = self._project.pad_content[sample_id]
            targets = ((content.instance_id, sample_id),) if content is not None else ()
        for content_id, current_slot in targets:
            self._poll_current_content(
                content_id, current_slot, report_unavailable=report_unavailable
            )

    def _poll_current_content(
        self, content_id: str, current_slot: int, *, report_unavailable: bool
    ) -> None:
        if self._is_observation_fenced(current_slot):
            return
        try:
            observation = self._transport._audio._current_ordinary_transport_observation(
                current_slot
            )
        except RuntimeError as error:
            if str(error) == "content feedback placement pending":
                return
            if (
                str(error) == "ordinary transport observation unavailable"
                and not report_unavailable
            ):
                return
            raise
        current_content = self._project.pad_content[current_slot]
        if current_content is None or current_content.instance_id != content_id:
            return
        if self._is_observation_fenced(current_slot):
            return
        if observation is not None and observation.sample_id == current_slot:
            self.project(observation)
        elif observation is None and (owner := self._owners.get(content_id)) is not None:
            invalid = self._retire_invalid_admission(
                owner, current_slot, report_unavailable=report_unavailable
            )
            if invalid:
                self._owners.pop(content_id, None)

    def project(self, observation: OrdinaryTransportObservation) -> None:
        """Apply only a Native-resolved current observation after Python mirror fences."""
        sample_id = observation.sample_id
        if self._is_observation_fenced(sample_id):
            return
        content = self._project.pad_content[sample_id]
        owner = self._owners.get(content.instance_id) if content is not None else None
        if (
            content is not None
            and owner is not None
            and observation.content_lifetime != owner.monitor.content_lifetime
        ):
            self._owners.pop(content.instance_id, None)
            owner = None
        if owner is not None and observation.observation_sequence < owner.observed_sequence:
            return

        if observation.active:
            self._session.active_sample_ids.add(sample_id)
        else:
            self._session.active_sample_ids.discard(sample_id)
        if observation.active and observation.ordinary_paused:
            self._session.paused_sample_ids.add(sample_id)
        else:
            self._session.paused_sample_ids.discard(sample_id)

        if owner is not None:
            owner.monitor = observation
            owner.observed_sequence = observation.observation_sequence
            self._retire_invalid_admission(owner, sample_id)
            if owner.admission is not None and observation.settles_admission(owner.admission):
                owner.pending = False
                owner.admission = None
        elif content is not None:
            self._owners[content.instance_id] = _PauseOwner(
                monitor=observation, observed_sequence=observation.observation_sequence
            )

    @staticmethod
    def _retire_invalid_admission(
        owner: _PauseOwner, sample_id: int, *, report_unavailable: bool = False
    ) -> bool:
        if owner.admission is None:
            return False
        try:
            current_slot = owner.admission.current_control_slot()
        except RuntimeError as error:
            if str(error) == "ordinary transport observation unavailable" and report_unavailable:
                raise
            if str(error) not in {
                "content feedback placement pending",
                "ordinary transport observation unavailable",
            }:
                raise
            return False
        if current_slot != sample_id:
            owner.admission = None
            owner.pending = False
            return True
        return False

    def _is_observation_fenced(self, sample_id: int) -> bool:
        return sample_id in self._transport._pair_deferred_sample_ids or (
            self._observation_fenced is not None and self._observation_fenced(sample_id)
        )

    def _prune_removed_content(self) -> dict[str, int]:
        live_slots = {
            content.instance_id: sample_id
            for sample_id, content in enumerate(self._project.pad_content)
            if content is not None
        }
        self._owners = {
            content_id: owner
            for content_id, owner in self._owners.items()
            if content_id in live_slots
        }
        return live_slots
