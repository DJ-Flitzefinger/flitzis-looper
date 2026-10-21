"""Sealed two-participant musical Move/Swap data, without live Native authority."""

from typing import Literal, Self

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.copy_snapshot import (
    CONTENT_FIELDS,
    MusicalSnapshotData,
    canonical_snapshot_json,
    musical_content_data,
    musical_snapshot_projection,
    strict_snapshot_sample_id,
)
from flitzis_looper.models import PadContentIdentity, ProjectState

type PairOperation = Literal["move", "swap"]


class PairSlotData(MusicalSnapshotData):
    """The complete Copy musical schema, also able to witness an empty slot."""

    @model_validator(mode="after")
    def _validate_occupancy(self) -> Self:
        if (self.source_path is None) != (self.source_identity is None):
            message = "Pair slot source and durable identity must have matching occupancy"
            raise ValueError(message)
        return self


class PairSnapshotData(BaseModel):
    """Detached participant projections; mutable children do not belong to the seal."""

    model_config = ConfigDict(extra="forbid", strict=True, revalidate_instances="always")

    operation: PairOperation
    source_id: int = Field(ge=0, lt=NUM_SAMPLES)
    target_id: int = Field(ge=0, lt=NUM_SAMPLES)
    source: PairSlotData
    target: PairSlotData

    @model_validator(mode="after")
    def _validate_participants(self) -> Self:
        if self.source_id == self.target_id:
            message = "Pair participants must name distinct slots"
            raise ValueError(message)
        if self.source.source_identity is None:
            message = "Pair source must contain loaded content with a durable identity"
            raise ValueError(message)
        target = self.target.source_identity
        if self.operation == "move" and target is not None:
            message = "Move target must be empty"
            raise ValueError(message)
        if self.operation == "swap" and target is None:
            message = "Swap target must contain loaded content with a durable identity"
            raise ValueError(message)
        if target is not None and target.instance_id == self.source.source_identity.instance_id:
            message = "Pair content instance identities must be distinct"
            raise ValueError(message)
        return self


class PairSnapshot(BaseModel):
    """Immutable complete musical pair seal, never a runtime placement permission."""

    model_config = ConfigDict(frozen=True, extra="forbid", revalidate_instances="always")

    schema_version: int = Field(default=1, strict=True, ge=1, le=1)
    payload_json: str = Field(strict=True, min_length=1)

    @field_validator("payload_json")
    @classmethod
    def _validate_and_seal_payload(cls, value: str) -> str:
        data = PairSnapshotData.model_validate_json(value, strict=True)
        return canonical_snapshot_json(data.model_dump(mode="json", warnings="error"))

    def projected(self) -> PairSnapshotData:
        """Revalidate even unchecked seals and return a fresh detached musical image."""
        validated = type(self).model_validate(self)
        return PairSnapshotData.model_validate_json(validated.payload_json, strict=True)

    @classmethod
    def from_data(cls, data: PairSnapshotData) -> Self:
        """Seal both complete participants after independent strict JSON validation."""
        return cls(
            payload_json=canonical_snapshot_json(data.model_dump(mode="json", warnings="error"))
        )


def _validate_unique_content_ids(project: ProjectState) -> None:
    seen: set[str] = set()
    for content in project.pad_content:
        if content is None:
            continue
        if not isinstance(content, PadContentIdentity):
            message = "Pair project contains an invalid content instance identity"
            raise TypeError(message)
        data = content.model_dump(mode="json", warnings="error")
        identity = PadContentIdentity.model_validate_json(
            canonical_snapshot_json(data), strict=True
        )
        if identity.instance_id in seen:
            message = "Pair project contains duplicate content instance identities"
            raise ValueError(message)
        seen.add(identity.instance_id)


def capture_pair_snapshot(
    project: ProjectState, source_id: int, target_id: int, *, operation: PairOperation
) -> PairSnapshot:
    """Capture complete current participants without I/O, mutation or live custody."""
    source_id = strict_snapshot_sample_id(source_id)
    target_id = strict_snapshot_sample_id(target_id)
    source = PairSlotData.model_validate(musical_content_data(project, source_id), strict=True)
    target = PairSlotData.model_validate(musical_content_data(project, target_id), strict=True)
    _validate_unique_content_ids(project)
    return PairSnapshot.from_data(
        PairSnapshotData(
            operation=operation,
            source_id=source_id,
            target_id=target_id,
            source=source,
            target=target,
        )
    )


def project_selected_pair_content(
    selected_pad: int, selected_content_id: str | None, pair: PairSnapshotData
) -> int:
    """Follow a chosen surviving content; empty and unrelated selections stay at their slots."""
    if selected_content_id is None:
        return selected_pad
    source = pair.source.source_identity
    target = pair.target.source_identity
    if source is not None and selected_content_id == source.instance_id:
        return pair.target_id
    if target is not None and selected_content_id == target.instance_id:
        return pair.source_id
    return selected_pad


def project_with_pair_snapshot(project: ProjectState, snapshot: PairSnapshot) -> ProjectState:
    """Project one strictly revalidated pair into the latest independent project image.

    Both captured participants must still match their complete musical seals.
    Unrelated current slots and global settings are taken from the supplied project.
    Stem availability is historical metadata only and remains unclaimed here.
    """
    captured = PairSnapshot.model_validate(snapshot).projected()
    current = capture_pair_snapshot(
        project, captured.source_id, captured.target_id, operation=captured.operation
    )
    if current != PairSnapshot.from_data(captured):
        message = "Pair participant musical content changed after capture"
        raise ValueError(message)
    source = (
        PairSlotData.model_validate(
            musical_content_data(ProjectState(), captured.source_id), strict=True
        )
        if captured.operation == "move"
        else captured.target
    )
    images = ((captured.source_id, source), (captured.target_id, captured.source))
    data = project.model_dump(mode="python", exclude_computed_fields=True, warnings="error")
    selected = project.pad_content[project.selected_pad]
    data["selected_pad"] = project_selected_pair_content(
        project.selected_pad, selected.instance_id if selected is not None else None, captured
    )
    for sample_id, image in images:
        projection = musical_snapshot_projection(image)
        for name, project_field in CONTENT_FIELDS:
            data[project_field][sample_id] = projection[name]
    projected = ProjectState.model_validate_json(canonical_snapshot_json(data), strict=True)
    _validate_unique_content_ids(projected)
    return projected
