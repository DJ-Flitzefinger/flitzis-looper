"""Complete musical bank images, without Native admission, ACK or live custody."""

from typing import Self
from uuid import uuid4

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

from flitzis_looper.constants import NUM_BANKS, NUM_PADS
from flitzis_looper.copy_snapshot import (
    CONTENT_FIELDS,
    CopySnapshot,
    CopySnapshotData,
    canonical_snapshot_json,
    copy_after_snapshot,
    musical_content_data,
)
from flitzis_looper.models import PadContentIdentity, ProjectState
from flitzis_looper.pair_snapshot import PairSlotData


def _bank_index(bank: int) -> int:
    if type(bank) is not int or not 0 <= bank < NUM_BANKS:
        message = "Bank index must be a strict integer naming an existing bank"
        raise ValueError(message)
    return bank


def _slot_image(project: ProjectState, sample_id: int) -> PairSlotData:
    return PairSlotData.model_validate(musical_content_data(project, sample_id), strict=True)


def _copy_after(source: PairSlotData, identity: PadContentIdentity) -> PairSlotData:
    snapshot = CopySnapshot.from_data(
        CopySnapshotData.model_validate(source.model_dump(mode="python"), strict=True)
    )
    copied = copy_after_snapshot(snapshot, identity).projected()
    return PairSlotData.model_validate(
        copied.model_dump(mode="python", warnings="error"), strict=True
    )


class BankSlotData(BaseModel):
    """One ordered source witness and complete target before/after musical image."""

    model_config = ConfigDict(extra="forbid", strict=True, revalidate_instances="always")

    source_id: int | None = Field(ge=0, lt=NUM_BANKS * NUM_PADS)
    target_id: int = Field(ge=0, lt=NUM_BANKS * NUM_PADS)
    source: PairSlotData | None
    before: PairSlotData
    after: PairSlotData
    resolved_key_lock: bool | None


def _validate_row_image(row: BankSlotData, empty: PairSlotData) -> None:
    source = row.source
    if source is None or source.source_identity is None:
        if row.after != empty or row.resolved_key_lock is not None:
            message = "Empty source and Clear entries require canonical empty target images"
            raise ValueError(message)
        return
    identity = row.after.source_identity
    if identity is None or row.resolved_key_lock is None:
        message = "Occupied bank copies require a fresh identity and Key Lock resolution witness"
        raise ValueError(message)
    if source.key_lock is not None and row.resolved_key_lock != source.key_lock:
        message = "Bank copy Key Lock witness changes its explicit source override"
        raise ValueError(message)
    if row.after != _copy_after(source, identity):
        message = "Bank after image changes captured musical content or fresh-copy lineage"
        raise ValueError(message)


def _validate_image_identities(slots: tuple[BankSlotData, ...]) -> None:
    original: set[str] = set()
    copied: set[str] = set()
    for row in slots:
        for image in (row.source, row.before):
            identity = None if image is None else image.source_identity
            if identity is not None:
                if identity.instance_id in original:
                    message = "Bank participants contain duplicate original content identities"
                    raise ValueError(message)
                original.add(identity.instance_id)
        identity = row.after.source_identity
        if identity is not None:
            if identity.instance_id in copied:
                message = "Bank after image contains duplicate fresh content identities"
                raise ValueError(message)
            copied.add(identity.instance_id)
    if original & copied:
        message = "Bank copies must have fresh identities distinct from every original participant"
        raise ValueError(message)


class BankSnapshotData(BaseModel):
    """Exactly one other-bank Copy or Clear image, including every empty participant."""

    model_config = ConfigDict(extra="forbid", strict=True, revalidate_instances="always")

    source_bank: int | None = Field(ge=0, lt=NUM_BANKS)
    target_bank: int = Field(ge=0, lt=NUM_BANKS)
    slots: tuple[BankSlotData, ...] = Field(min_length=NUM_PADS, max_length=NUM_PADS)

    @model_validator(mode="after")
    def _validate_complete_bank(self) -> Self:
        if self.source_bank == self.target_bank:
            message = "Bank Copy requires a different target bank"
            raise ValueError(message)
        defaults = ProjectState()
        for offset, row in enumerate(self.slots):
            target_id = self.target_bank * NUM_PADS + offset
            source_id = None if self.source_bank is None else self.source_bank * NUM_PADS + offset
            if (
                row.target_id != target_id
                or row.source_id != source_id
                or (row.source is None) != (self.source_bank is None)
            ):
                message = "Bank rows must name all 36 corresponding slots in their exact bank order"
                raise ValueError(message)
            _validate_row_image(row, _slot_image(defaults, target_id))
        _validate_image_identities(self.slots)
        return self


class BankSnapshot(BaseModel):
    """Canonical immutable musical seal; it grants no runtime or restart authority."""

    model_config = ConfigDict(frozen=True, extra="forbid", revalidate_instances="always")

    schema_version: int = Field(default=1, strict=True, ge=1, le=1)
    payload_json: str = Field(strict=True, min_length=1)

    @field_validator("payload_json")
    @classmethod
    def _validate_and_seal_payload(cls, value: str) -> str:
        data = BankSnapshotData.model_validate_json(value, strict=True)
        return canonical_snapshot_json(data.model_dump(mode="json", warnings="error"))

    def projected(self) -> BankSnapshotData:
        """Revalidate unchecked wrappers and return detached complete musical children."""
        validated = type(self).model_validate(self)
        return BankSnapshotData.model_validate_json(validated.payload_json, strict=True)

    @classmethod
    def from_data(cls, data: BankSnapshotData) -> Self:
        """Seal only a fully revalidated, independently serialized complete bank image."""
        return cls(
            payload_json=canonical_snapshot_json(data.model_dump(mode="json", warnings="error"))
        )


def _project_content_ids(project: ProjectState) -> set[str]:
    seen: set[str] = set()
    for content in project.pad_content:
        if content is None:
            continue
        if not isinstance(content, PadContentIdentity):
            message = "Bank project contains an invalid content identity"
            raise TypeError(message)
        identity = PadContentIdentity.model_validate_json(
            canonical_snapshot_json(content.model_dump(mode="json", warnings="error")), strict=True
        )
        if identity.instance_id in seen:
            message = "Bank project contains duplicate content identities"
            raise ValueError(message)
        seen.add(identity.instance_id)
    return seen


def capture_bank_snapshot(
    project: ProjectState, source_bank: int | None, target_bank: int
) -> BankSnapshot:
    """Capture all participants and fresh stopped musical images without reading artifacts.

    A None source bank captures Clear. Copy retains shared immutable material and
    original musical metadata, while every occupied target gets a new instance.
    Empty source slots are witnessed in full and clear their target to defaults.
    Current stem availability, voices, Holds, jobs and Native permissions are excluded.
    """
    target_bank = _bank_index(target_bank)
    if source_bank is not None:
        source_bank = _bank_index(source_bank)
    if source_bank == target_bank:
        message = "Bank Copy requires a different target bank"
        raise ValueError(message)
    _project_content_ids(project)
    defaults = ProjectState()
    rows = []
    for offset in range(NUM_PADS):
        target_id = target_bank * NUM_PADS + offset
        source_id = None if source_bank is None else source_bank * NUM_PADS + offset
        source = None if source_id is None else _slot_image(project, source_id)
        after = _slot_image(defaults, target_id)
        resolved = None
        if source is not None and source.source_identity is not None:
            original = source.source_identity
            identity = PadContentIdentity(
                instance_id=uuid4().hex,
                material_id=original.material_id,
                material_creation=original.material_creation,
                copied_from=original.instance_id,
            )
            after = _copy_after(source, identity)
            resolved = project.key_lock if source.key_lock is None else source.key_lock
        rows.append(
            BankSlotData(
                source_id=source_id,
                target_id=target_id,
                source=source,
                before=_slot_image(project, target_id),
                after=after,
                resolved_key_lock=resolved,
            )
        )
    return BankSnapshot.from_data(
        BankSnapshotData(source_bank=source_bank, target_bank=target_bank, slots=tuple(rows))
    )


def _validate_current_participants(project: ProjectState, captured: BankSnapshotData) -> None:
    current_ids = _project_content_ids(project)
    for row in captured.slots:
        if _slot_image(project, row.target_id) != row.before or (
            row.source_id is not None and _slot_image(project, row.source_id) != row.source
        ):
            message = "Bank participant musical content changed after capture"
            raise ValueError(message)
        if row.source is not None and row.source.source_identity is not None:
            resolved = project.key_lock if row.source.key_lock is None else row.source.key_lock
            if resolved != row.resolved_key_lock:
                message = "Bank source Key Lock inheritance changed after capture"
                raise ValueError(message)
        identity = row.after.source_identity
        if identity is not None and identity.instance_id in current_ids:
            message = "Bank target identity is no longer fresh in the current project"
            raise ValueError(message)


def validate_current_bank_snapshot(
    project: ProjectState, snapshot: BankSnapshot
) -> BankSnapshotData:
    """Revalidate the original seal and current participants without building an after image.

    This read-only check grants no Native, transaction or durable authority. The
    returned detached data may be used by the existing all36 projection only.
    """
    captured = BankSnapshot.model_validate(snapshot).projected()
    _validate_current_participants(project, captured)
    return captured


def project_with_bank_snapshot(project: ProjectState, snapshot: BankSnapshot) -> ProjectState:
    """Project all 36 rows only after every captured participant still matches.

    The supplied project's latest unrelated content, globals and selected slot/bank
    are preserved. Physical-slot MIDI bindings are outside this musical schema.
    This returns an independent candidate; it neither mutates the project nor claims
    a Native effect, durable commit or ACK.
    """
    captured = validate_current_bank_snapshot(project, snapshot)
    data = project.model_dump(mode="python", exclude_computed_fields=True, warnings="error")
    for row in captured.slots:
        image = row.after.model_dump(mode="python", warnings="error")
        for name, project_field in CONTENT_FIELDS:
            data[project_field][row.target_id] = image[name]
    candidate = ProjectState.model_validate_json(canonical_snapshot_json(data), strict=True)
    _project_content_ids(candidate)
    return candidate
